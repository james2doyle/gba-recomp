# ADR-003 — Tempo guest determinístico e preferências de apresentação

- Estado: Proposed
- Data: 2026-10-13

## Contexto

Timing de DMA, FIFO, timers e raster afeta resultado guest. Taxa de atualização do monitor e dispositivo de áudio variam entre hosts.

## Decisão proposta

Scheduler em ciclos inteiros, checkpoints intra-bloco e efeitos de barramento explícitos. Preferências de vídeo/áudio de apresentação não alteram hardware guest. Ajustes de fidelidade, RTC e overrides de cartucho entram no digest semântico do perfil. Fast-forward altera pacing host; cheats e turbo são modificadores de input declarados.

## Alternativas e consequências

Somar ciclos fixos apenas ao fim do bloco é mais barato, mas pode atravessar eventos observáveis. Fixar o jogo em 60 Hz acompanha monitores, mas muda seu relógio. Baseline preciso exige mais checkpoints; otimizações precisam equivalência demonstrada.

## Revisão

Revisitar granularidade de execução somente com suites de timing e cenários sensíveis. Ver [SPEC-005](../specs/SPEC-005-runtime-timing.md) e [SPEC-009](../specs/SPEC-009-settings-menu.md).
