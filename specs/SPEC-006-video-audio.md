# SPEC-006 — PPU, APU e apresentação

Estado do documento: Draft. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Problema e escopo

CPU nativa deve produzir vídeo e áudio equivalentes ao hardware modelado. Efeitos de apresentação são preferências host e não modificam a simulação.

## Requisitos normativos

- VID-01: PPU baseline MUST produzir framebuffer bruto 240×160 em formato de 15 bits com layout de canais documentado e sem ambiguidade. Modos 0–5, BG texto/affine/bitmap, OBJ/OAM, prioridades, janelas, blend, mosaic, forced blank, page flip e VRAM/palette MUST estar na matriz de cobertura.
- VID-02: scanlines e eventos MUST seguir SPEC-005. Writes durante desenho MUST ser aplicados ao segmento temporal correto ou gerar capability ausente; renderizar toda linha só com estado final não pode ser chamado preciso sem validação.
- VID-03: escala inteira, nearest/linear, proporção 3:2, letterbox, correção de cor e persistência LCD MUST operar após framebuffer bruto. Histórico temporal avança por frame guest, inclusive se a apresentação pular frames.
- VID-04: vsync e refresh variável MUST afetar apresentação/pacing, não substituir frequência guest por 60 Hz. Nenhum renderer pode exigir API específica no código recompilado.
- AUD-01: APU MUST modelar quatro canais PSG e dois FIFOs direct sound, registradores, envelopes/sweep/length, wave RAM, ruído, routing, timers/DMA, SOUNDBIAS e master enable conforme cobertura explícita.
- AUD-02: geração de amostras canônicas MUST depender somente do tempo guest e estado APU. Resampling host, volume, mute e latência não alteram timers/FIFOs nem removem execução guest.
- AUD-03: backend MUST usar buffer limitado, reportar underrun/overrun e negociar sample rate; dispositivo ausente mantém simulação funcionando em silêncio. Troca de dispositivo não perde estado APU.
- AV-01: hashes de verificação MUST usar framebuffer antes de filtros e áudio canônico antes de volume/resampling host. Parâmetros desse áudio canônico são fixados no manifesto de teste.

## Interfaces e erros

`Frame { guest_cycle, frame_index, pixels }`; `AudioChunk { first_guest_cycle, format_id, samples }`. Filas limitadas e ownership explícito. `video_backend_unavailable` pode ativar backend alternativo anunciado; `audio_device_lost` oferece recuperação sem reiniciar o jogo.

## Aceite e matriz

Vídeo: cada modo, sprites affine, wrapping, blending, prioridade, janelas e writes em meio de linha. Áudio: envelopes, ruído, FIFO underrun/refill por timer, DMA e mudanças de routing. Produto: resize, DPI, tela cheia, mute, desconexão, monitor 60/120 Hz. Mesmos inputs e ciclos MUST manter hashes canônicos quando preferências host mudam.

## Limites e referências

Áudio enhanced por substituição de engine não pertence ao baseline; requer contrato por jogo e não serve como oráculo de fidelidade. Fontes KNO-001 SRC-05/07/10 e mGBA `src/gba/renderers/`, `audio.c`, `video.c`.
