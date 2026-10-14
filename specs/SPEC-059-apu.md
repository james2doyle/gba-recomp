# SPEC-059 — APU do núcleo `gba-core`

Estado do documento: Accepted em 2026-10-14. Escrita junto com a primeira implementação (ADR-041). Progresso: [ROADMAP](../ROADMAP.md).

## Problema e escopo

Implementar AUD-01/AUD-02 da [SPEC-006](SPEC-006-video-audio.md): quatro canais PSG, dois FIFOs de direct sound alimentados por timer/DMA e mixer, gerando áudio canônico dependente só do tempo guest.

## Requisitos normativos

- APU-59-01: saída canônica estéreo i16 a 32768 Hz (uma amostra a cada 512 ciclos guest), produzida pelo scheduler, nunca pelo relógio host.
- APU-59-02: frame sequencer a 512 Hz: length a 256 Hz, sweep a 128 Hz, envelope a 64 Hz.
- APU-59-03: canais 1/2 square com duty, envelope, length e sweep (canal 1); canal 3 wave com dois bancos, dimensão 64 amostras, volumes 0/100/50/25 e forçado 75%; canal 4 ruído LFSR 15/7 bits com divisor e shift.
- APU-59-04: FIFO A/B de 32 bytes; overflow do timer selecionado em SOUNDCNT_H consome uma amostra; com ≤ 16 bytes restantes solicita DMA1/DMA2 em modo especial (4 words, destino fixo). Underrun MUST ser contado.
- APU-59-05: mixer aplica SOUNDCNT_L/H (volumes, roteamento L/R, PSG 25/50/100%, FIFO 50/100%) e SOUNDBIAS com clamp de 10 bits; master off MUST zerar e travar registradores PSG.
- APU-59-06: wave RAM acessível pela CPU no banco não selecionado para reprodução.

## Limites

Sem modelagem do PWM/resolução de SOUNDBIAS (amostragem fixa 32768 Hz), sem latência de escrita sub-amostra. Resampling, volume e dispositivo ficam no host (AUD-02/03).

## Critérios de aceite

Testes de envelope/length/sweep, FIFO underrun/refill por timer+DMA e roteamento; captura WAV canônica comparável entre execuções com o mesmo input.

## Referências

[SPEC-006](SPEC-006-video-audio.md), [SPEC-057](SPEC-057-bus-scheduler-io.md), GBATEK.
