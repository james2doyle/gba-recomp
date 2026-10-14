# SPEC-058 — PPU por software do núcleo `gba-core`

Estado do documento: Accepted em 2026-10-14. Escrita junto com a primeira implementação (ADR-041); a ordem spec→código não foi seguida nesta etapa e fica registrada aqui. Progresso: [ROADMAP](../ROADMAP.md).

## Problema e escopo

Implementar o baseline VID-01/VID-02 da [SPEC-006](SPEC-006-video-audio.md): renderer por scanline que produz o framebuffer bruto 240×160 BGR555 consumido por hashes (AV-01) e pelo frontend.

## Requisitos normativos

- PPU-58-01: timing de linha 1232 ciclos (HDraw 1008 + HBlank 224), 228 linhas; VBlank nas linhas 160–226; flags/IRQs de VBlank, HBlank e VCount match em DISPSTAT; HBlank DMA só em linhas visíveis.
- PPU-58-02: cada linha visível MUST ser renderizada no início do HBlank com o estado de registradores daquele instante (writes durante HDraw só afetam a linha seguinte; limitação de VID-02 registrada).
- PPU-58-03: modos 0–5: BG texto 4/8 bpp com tamanhos 256/512, flip e paletas; BG affine com wrap; bitmaps 3/4/5 com page flip e transformação BG2; mosaic de BG.
- PPU-58-04: OBJ normal e affine (double size), 4/8 bpp, mapeamento 1D/2D, prioridade entre OBJs pelo índice OAM, modo semi-transparente, OBJ window, mosaic de OBJ, tiles < 512 indisponíveis em modos bitmap.
- PPU-58-05: janelas WIN0/WIN1/OBJWIN/outside com máscaras por camada e efeitos; blend alpha/brighten/darken com EVA/EVB/EVY saturados em 16; forced blank produz branco.
- PPU-58-06: referências affine internas MUST ser recarregadas em escrita de BGxX/BGxY e no VBlank e avançadas por PB/PD a cada linha.
- PPU-58-07: writes de byte em Palette e VRAM BG duplicam o byte; em OAM e VRAM OBJ são ignoradas (BUS-57-02).

## Limites

Sem ciclos de acesso VRAM por pixel, sem contenção CPU/PPU, sem renderização no meio da linha. Hash canônico: FNV-1a do framebuffer bruto.

## Critérios de aceite

Testes de composição por camada/prioridade, janelas e blend; comparação de framebuffer com mGBA (oráculo externo) nos mesmos frames de fixtures quando o diferencial existir. Evidência inicial: LeafGreen renderiza intro, título, tutorial e diálogo (KNO-007).

## Verificação

Testes de cena sintética em `crates/gba-core/src/ppu/tests.rs` (26 testes, 2026-10-14) montam VRAM/Palette/OAM/I/O pelo bus e conferem pixels após renderizar uma linha: BG texto 4/8 bpp, flips, banco de paleta, scroll com wrap, largura 512, prioridade entre BGs, mosaic H/V; BG affine com e sem wrap, ponto de referência (inclusive negativo de 28 bits), escala, avanço por linha e recarga no VBlank; modos 3/4/5 com page flip e limites do modo 5; OBJ normal (flips, X/Y negativos), 8 bpp, mapeamento 1D vs 2D, prioridade OBJ×OBJ e OBJ×BG, affine identidade/double size/espelho por PA, tiles < 512 em modo bitmap, semi-transparência, OBJ window; WIN0/WIN1 (prioridade e limites verticais), alpha com EVA/EVB saturados, backdrop como segundo alvo, brighten/darken, janela desabilitando efeitos e forced blank. Flags/IRQs de DISPSTAT e HBlank DMA estão em `bus/tests.rs`. Mosaic de OBJ e a comparação com mGBA continuam sem teste. Bugs corrigidos: [KNO-007](../knowledge/KNO-007-interpreted-core.md#bugs-corrigidos-pelos-testes-unitários).

## Referências

[SPEC-006](SPEC-006-video-audio.md), [SPEC-057](SPEC-057-bus-scheduler-io.md), [ADR-041](../adr/ADR-041-interpreter-first-core.md), GBATEK.
