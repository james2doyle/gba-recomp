# KNO-011 — Levantamento de apresentação ampliada (widescreen) para GBA

Data: 2026-10-24. Escopo: contrato de apresentação. **Não** é implementação, não é evidência de execução de jogo e não autoriza pacote com imagem ampliada.

Este documento registra o que foi lido, o que foi executado e o que permanece hipótese, antes de qualquer alteração em `crates/gba-core`. Serve de base para [SPEC-069](../specs/SPEC-069-widescreen-presentation.md) e [ADR-042](../adr/ADR-042-widescreen-presentation-surface.md).

IDs locais `WS-nn` são deste levantamento; não substituem `SRC-nn` de [KNO-001](KNO-001-evidence.md) e precisam ser promovidos ao registro daquele arquivo antes de virarem referência normativa.

## Conclusão

O GBA não possui modo de vídeo com mais de 240 colunas visíveis. Nenhum emulador de referência implementa ampliação horizontal: mGBA mantém o pedido aberto, ares/NanoBoyAdvance/SkyEmu/BizHawk renderizam 240 fixo, e o próprio vendor `JRickey/gba-recomp` — que investe pesado em aparência (cor por revisão de painel, resposta temporal, grade de subpixel) — mantém framebuffer, textura, buffers temporais e apresentação em 240×160.

A única implementação aberta com saída horizontal maior que 240 é `mstan/gbarecomp`, e ela não amplia o renderer fiel: mantém o caminho nativo byte-idêntico e adiciona um segundo compositor opt-in, mais *providers* por jogo. A conclusão prática é que **ampliar é fácil; preencher a margem com mundo é um problema por jogo**, e não existe artefato genérico que resolva o segundo.

Consequência para este projeto: a superfície ampliada pode ser especificada agora como capacidade opt-in de apresentação, desde que o framebuffer canônico continue sendo a única fonte de hash e de verificação (VID-01, VID-03, AV-01, VAL-02).

## Observação de fonte

### `JRickey/gba-recomp` — vendor fixado em KNO-006

Commit `de4edf59b872d887046d6a3b005e2df551b6d44c` (2026-06-18, `release: fix macOS fallback bundle layout`); `pushed_at` 2026-06-18, sem push posterior até 2026-10-24. Mesmo commit de [KNO-006](KNO-006-vendor-gba-recomp.md).

| ID | Confiança | Observação | Consequência |
|---|---|---|---|
| WS-01 | verified | `crates/gba-core/src/ppu.rs`: `use crate::mem::{MemMap, VISIBLE_WIDTH};`, `type Line = [u16; VISIBLE_WIDTH];`, `bg_lines`, `obj_line`, `obj_window` todos dimensionados por `VISIBLE_WIDTH`; escrita `mem.framebuffer[row + x] = color;` com `let row = line * VISIBLE_WIDTH;`. Não existe identificador `MARGIN`, `WIDE`, `EXTRA`, `BORDER`, `OVERSCAN` nem `pitch`/`stride` | O vendor tem exatamente a mesma forma do nosso `ppu.rs`: largura por constante, sem conceito de margem |
| WS-02 | verified | `crates/gba-core/src/mem.rs`: `framebuffer: vec![0; VISIBLE_WIDTH * VISIBLE_SCANLINES as usize]`, comentário `/// 240×160 RGB555 output`; nenhum outro buffer de vídeo, nenhum campo de pitch | Um `Vec` plano; ampliar exige ou stride explícito ou buffer separado |
| WS-03 | verified | `crates/screen/src/present.rs`: `Presenter::new(window, src_w, src_h, target, vsync)` guarda `src_size: (u32, u32)` **sem setter**; textura criada em `src_w × src_h`; `debug_assert_eq!(rgba.len(), (sw * sh) as usize)`; geometria derivada por `let scale = ((w as f32) / (sw as f32)).min((h as f32) / (sh as f32));` com `origin` centralizado e `LoadOp::Clear(BLACK)` | A apresentação é **agnóstica à largura**: não existe `3.0/2.0` nem 240 no cálculo. Uma fonte 384×160 já seria centralizada e letterboxed sem mudança de código |
| WS-04 | verified | `crates/recomp/src/play.rs`: `Presenter::new(window.clone(), 240, 160, …)`, `rgba: vec![[0u8; 4]; 240 * 160]`, `Temporal::new(240 * 160, …)`, fallback `let (sw, sh) = (240u32, 160u32);`, `menu.draw(&self.av, &mut self.rgba, 240, 160)`; janela inicial `240.0 * 3.0 × 160.0 * 3.0`, `F11` = borderless | A largura fixa está nos buffers do frontend e nos menus, não na apresentação |
| WS-05 | verified | `crates/launcher/src/screens/av.rs`: tela A/V expõe `screen` (RAW/ORIGINAL/FRONTLIT/BACKLIT/CLASSIC), `screen_darken`, `response` (OFF/BLEND/SMART/GHOST), `grid`, `display_gamut`. Sem proporção, escala, resolução ou ampliação | A verba de "enhancement" do vendor foi para aparência da imagem 240×160, não para mais imagem |
| WS-06 | verified | `crates/pack/src/config.rs`: `#[serde(deny_unknown_fields)]` em `PackConfig` e em cada seção; `[runtime]` = `menu`, `enhanced-audio`, `screen-sim`, `engine-hle`, `interpreter` | Um `view-width` num `pack.toml` antigo falharia alto, não silenciosamente — mesma política do nosso parser |
| WS-07 | verified | `docs/architecture.md`: *"Frame hashes / verify / sweeps stay defined on raw BGR555 — the pipeline is present-time only"*; pipeline `raw BGR555 → temporal → LUT → grid+scale → present`, com resposta temporal avançando no frame **emulado**, não no apresentado | Segunda fonte independente para a separação que a SPEC-069 adota |
| WS-08 | verified | `crates/gamedb/src/lib.rs`: `functions(sha256, address, end, mode, name)` + `gamedb_meta(key, value)`, aberto `SQLITE_OPEN_READ_ONLY`, consulta `WHERE sha256 = ?1`; nenhum metadado por jogo além do hash | O padrão "dado versionado atrelado ao SHA-256" (SPEC-008) já é utilizado por terceiro; adapters de margem cabem em `profiles/`, não em banco novo |
| WS-09 | verified, negative | Busca `repo:JRickey/gba-recomp widescreen OR aspect` em issues/PRs: `total_count: 0` | Não há trabalho alheio em andamento que possamos aguardar |

Limitações desta leitura: o texto chegou por extração HTML, com corrupção local visível em alguns trechos (por exemplo `while sx = 0x1_0000 {`, cabeçalho do laço de `compose` ausente, corpo de `in_window_range` truncado). As afirmações acima usam apenas trechos literais intactos; nada foi reconstruído por inferência de operador.

Dois fatos de `ppu.rs` que valem como armadilha, ambos `verified`:

- `let mut x = (a1 & 0x1FF) as i32; if x >= 240 { x -= 512; }` é **decodificação** do campo X de 9 bits com sinal, não recorte. Ampliar não pode tocar esse limiar, ou sprites com X em 240–255 viram negativos.
- `fn in_window_range(x1: u8, x2: u8, v: u8)` é chamado com `x as u8`, truncando em 256. O vendor não teria como expressar janela acima de 255 sem mudar a assinatura. Nosso `window_mask` usa `usize` e já limita `x2` a 240 (`ppu.rs:529`), que é o comportamento conservador correto.

### `mstan/gbarecomp` — única implementação aberta com margem

Licença PolyForm Noncommercial 1.0.0: **referência de desenho apenas, sem reutilização de código** (regra 11 de `AGENTS.md`).

| ID | Confiança | Observação | Consequência |
|---|---|---|---|
| WS-10 | verified (cabeçalho) | `src/gba/gba_ppu.h`: *"With every margin 0 the PPU runs the LITERAL vanilla path (`render_scanline_internal`), so OFF-mode is byte-identical to the faithful build BY CONSTRUCTION — not by algebraic equivalence"*; `kMaxExtraX = 520`, `kMaxRenderWidth = 1280`, `kMaxExtraY = 352`; *"These margins are present-time host state and are NEVER serialized into the snapshot"* | Confirma o desenho: segundo caminho, não parametrização do fiel; margem fora de qualquer save/hash |
| WS-11 | verified (cabeçalho) | Providers declarados como globais opcionais, todos nulos por padrão: `g_ws_tilemap_provider`, `g_ws_bg_x_provider`, `g_ws_bg_xy_provider`, `g_ws_ui_pixel_provider`, `g_ws_affine_filter_provider`, `g_ws_obj_x_provider`, `g_ws_obj_attr_x_provider`, `g_ws_obj_attr_y_provider`, `g_ws_obj_margin_provider`, além de `g_ws_obj_native_clip`, `g_ws_authored_margin_layers`, `g_ws_pillarbox`, `g_ws_native_view_left` | A margem correta é **dado fornecido por jogo**, não resultado de ampliar o renderer |
| WS-12 | secondhand | `.cpp`/planos: BG texto amostra o anel 256/512 e portanto repete; janelas não são ampliadas e margem cai em WINOUT, com falha fechada quando o controle de janela não é uniforme na linha; bitmap para na borda 240/160; OBJ é decodificado em 9 bits e testado contra o viewport ampliado | Consistente com nossa leitura de `ppu.rs`; **não verificado nesta sessão** por corrupção da extração do `.cpp` |
| WS-13 | secondhand | Documentação de jogo: FireRed/LeafGreen exigem tilemap de 64 tiles (o de campo tem 32 e envolve mod 256), deslocamento de screenblocks e alargamento de buffers no heap do guest; margens ficam visivelmente erradas durante scroll e o recurso vem desligado | O caminho "512-wide text map" é mudança no guest, não no nosso PPU — território da SPEC-008 |
| WS-14 | secondhand | HUD, culling e câmera foram tratados por jogo (âncoras de OAM, reescrita de comparações de culling em PCs exatos, provedores de margem) | Adapters são trabalho por título, com evidência própria |

### mGBA — por que a ampliação ingênua falha

| ID | Confiança | Observação | Consequência |
|---|---|---|---|
| WS-15 | secondhand | Issue #1960 (aberta, milestone 1.1.0) e #2937 fechada como duplicata; mantenedor: *"there are also a bunch of complexities … the inherent limit of 256 pixels wide that would need modifications in the MMIO API"*. Relato de tentativa: acumulador X `uint8_t` limita a 255 colunas; passar disso exige `uint16_t` e "artefatos no renderer de software" | O limite de 256 do mapa de texto não é ajustável por constante; é contrato de API |

## Reprodução e resultado

Duas sondas descartáveis, executadas nesta máquina em 2026-10-24. Protótipo em cópia fora da árvore (`/tmp/wsprobe`), **nenhum artefato versionado** e nenhuma alteração em `crates/`; `git status` permaneceu limpo exceto por `.devshots/` pré-existente.

Ambiente: Linux 7.3.0-2-cachyos, AMD Ryzen 7 PRO 5850U (16 CPUs lógicas), rustc 1.99.0, build `--release`. ROM: `games/rkanoid-v3.00.gba`, SHA-256 `99ae825ffb1b7e8b48b33cd2715ceb8ca524858f99fee4b22938489460083d94`.

| ID | Cenário | Resultado |
|---|---|---|
| WS-16 | Cópia de `gba-core` com **apenas** `SCREEN_WIDTH` 240 → 320; Rkanoid, 400 frames; leitura de colunas 240–319 da linha 0–159 | Colunas 240–255: 1 cor única (backdrop). Colunas 256–319: **64/64 idênticas à coluna `x-256`** — o anel do tilemap. `BG0CNT=0x00C7`, `BG1CNT=0x0285` (ambos mapa 256×256) |
| WS-17 | Mesmo build; hash FNV-1a da janela canônica (240 colunas) extraída do buffer de stride 320 | `7ca201145fd0213f`, idêntico ao `frame_hash` de `gba-run` para o frame canônico. Confirma que ampliar o *stride* não altera o conteúdo guest-visível |
| WS-18 | Diferencial de 1200 frames, build canônico vs build de stride 320 (mesmos inputs: RIGHT 60–200, A 300–340, START 600–640) | **0 frames divergentes** na janela 240; hash final `36760c749c9436f4` nos dois lados; buffer ampliado completo `56c2cee7fce4f33d` |
| WS-19 | Custo, 2000 frames, mesmo ROM, largura do buffer | 240 → 796 fps; 288 → 760; 320 → 724; 384 → 678. Linear em colunas (~15% em 384) |
| WS-20 | OAM do título no frame 400 (RIGHT a partir do frame 60), 128 entradas | 128 objetos habilitados com `attr0=0x20A8` (Y=168) e X bruto 248, ou seja estacionados **abaixo** da área visível (Y ≥ 160), não à direita. X=248 está dentro de `0..256`, portanto **não** é instância do idioma "X ≥ 240 ⇒ negativo". O idioma de sprite estacionado existe no título, mas esta sonda não demonstrou caso horizontal e não mediu OBJ em margem |

Reprodução das sondas: copiar `crates/gba-core` para fora da árvore, ajustar `SCREEN_WIDTH`, comparar hash FNV-1a da janela canônica com `gba-run --frames N`. Comandos exatos e saídas estão na sessão que gerou este documento; como o protótipo era descartável, não há artefato em `knowledge/evidence/`.

Interpretação das sondas, separando o que foi medido do que é inferência:

- **Medido:** ampliar largura não muda a imagem canônica em 1200 frames com input; a margem é wrap de tilemap + backdrop; custo é linear em colunas.
- **Inferido:** que a margem de um jogo comercial (mapa 512, câmera mais larga, HUD reposicionado) também apareça depende de mudança no guest ou de adapter — as sondas não cobrem esse caso e não há fixture que cubra.
- **Não medido:** modo affine com referência deslocada, janelas com `>= 240`, bitmap com viewport ampliado, interação com `gba-native-rt`/AOT.

## Implementação (GBA-105, 2026-10-24)

O contrato de [SPEC-069](../specs/SPEC-069-widescreen-presentation.md) foi implementado em `gba-core`, `gba-desktop`, `gba-settings`, `gba-run` e `gba-native-rt`. Resultados medidos nesta máquina (host Linux x86-64, Ryzen 7 PRO 5850U, rustc 1.99.0), registrados em [evidência](evidence/gba105-widescreen-linux-x86_64.json):

| ID | Verificação | Resultado |
|---|---|---|
| WS-21 | Identidade canônica com `View::NONE` (T-01) | Seis cenas sintéticas (modos 0–5) batem com hashes capturados **antes** da alteração: `82d91296c6da8689`, `00d462ecb6011325`, `10ad2b31b360a525`, `5d7be25f393c9725`, `9d770531c80fed25`, `174069d99ad73165` |
| WS-22 | Diferencial de frame na ROM fixada (T-20) | `gba-run --hashes` com 1200 frames: **1200/1200 idênticos** entre 240, 320 e 384 colunas; 45 hashes distintos no baseline confirmam que o diferencial não compara uma imagem estática |
| WS-23 | Não-interferência (T-22) | `instructions_interpreted` e `guest_cycles` idênticos com e sem margem (`28 146 361` instruções, `336 991 441` ciclos) |
| WS-24 | Recorte canônico (T-20b) | O recorte de 240 colunas da captura 320×160 é **byte-idêntico** ao PPM canônico de `gba-run` na mesma ROM e frame |
| WS-25 | Conteúdo da margem (confirma WS-16 com o código real) | Em 320×160: 11 716/12 800 px de margem são backdrop; margem direita repete a faixa nativa em 5 488/6 400 px (anel de 256). Não é mundo novo |
| WS-26 | Apresentação headless (G-D) | `SDL_VIDEODRIVER=dummy` em 600 frames para 240/320/384: `presented=600`, `underruns=0`, `overruns=0` |
| WS-29 | Sincronia entre `--view-width` e a UI (correção pós-revisão) | Segundo bug real reportado: o menu exibia a preferência persistida (240) enquanto a superfície era 320, e `Apply` reescrevia 240 no arquivo, descartando o override em silêncio. Correção: o menu recebe o override (`Menu::set_view_override`), rotula `320 px (forcado)`, e uma edição explícita supersede o override enquanto um **Apply sem edição não persiste o override**; o menu nativo marca o item forçado; `gba-settings` recebe `--session-view-width` como aviso, já que é outro processo. Testes: `cli_override_is_shown_and_not_persisted` e `editing_the_width_row_persists_and_drops_the_override` |
| WS-28 | Enquadramento do overlay (correção pós-revisão) | Bug real encontrado pelo usuário: `Video::new` inicializava `view_left: 0` e `set_view_width` nunca era chamado, então o menu era desenhado nas colunas 0–239 de uma superfície de 320 e aparecia fora de centro. Correção: `view_left` vem da criação, `set_view_width` é acionado no apply, o backdrop usa `fill_surface` (cobre as margens) e `Painter` desloca o conteúdo guest. Medido no render simulado: painel guest 2..238 cai em 42..277, centro 159.5 == centro do recorte canônico |
| WS-27 | Janela real em Wayland | `gba-desktop --frames 120 --view-width 320` na sessão Wayland (`DISPLAY=:0`, `WAYLAND_DISPLAY=wayland-0`): janela abre, `presented=120`, sem erro. Abertura e apresentação verificadas; **inspeção visual do conteúdo** e alinhamento de toque físico seguem manuais |

Correções de teste que valem registro, porque todas foram erro de fixture e não do renderer: em `T-03` o bloco de tela 31 aliasava para dentro do mapa 512; em `T-04` a cor esperada não havia sido escrita; em `T-08` a tile de OBJ só tem pixels em x=0 e x=7; em `T-08b` lia-se a linha 0 em vez da 10; e `BLDCNT=0x0040` não torna BG0 alvo primário. Também foi encontrado um bug real: `store_line` gravava `view_framebuffer` mas não `view_line`, deixando a linha ampliada obsoleta.

O que **não** foi verificado: janela real e alinhamento de toque físico (host sem display), menus na tela, apresentação Web/iOS/Android (as duas amarras de 3:2 seguem intactas), pacote AOT ampliado de ponta a ponta e qualquer adapter de margem por jogo (GBA-110 continua bloqueado por falta de título-alvo com SHA fixado).

## Até onde o framework vai (WS-30, medido)

Uma pergunta legítima: a superfície ampliada resolve algo além de apresentação? A resposta medida é que existe **exatamente uma** classe de conteúdo em que o framework entrega mundo novo sem tocar no guest: BG **affine** com mapa grande.

| ID | Verificação | Resultado |
|---|---|---|
| WS-30 | Mapa affine 1024×1024, texel 296 apontando para um tile distinto | O recorte canônico de 240 colunas nunca contém a cor; a margem contém. O mapa tem conteúdo real além da janela nativa, então aqui a ampliação não é anel nem backdrop |
| WS-30b | Mesmo cenário, comparando superfície deslocada | A coluna de hardware 0 cai na coluna de saída 16 e a margem continua a mesma amostragem, provando que a extrapolação usa `hw_x` |
| WS-31 | Limite do mapa | `size = 128 << (cnt >> 14)`: máximo 1024 texels. Além disso não há como o PPU produzir conteúdo; é o teto físico |

O que **não** vem do framework, e portanto depende do código do jogo:

- **BG de texto**: o mapa tem 256 ou 512 texels; a margem é enrolamento do anel ou backdrop. Um mapa 512 é conteúdo real, mas exige que o jogo *use* um mapa 512 — não escolha do host.
- **Bitmap (modos 3/4/5)**: 240×160 por construção (160×128 no modo 5). Não existe mapa maior.
- **Câmera, HUD, culling e "sprites estacionados"**: decididos pelo guest. Ampliar o viewport mostra HUD fora de lugar e sprites parqueados reaparecendo; corrigir exige adapter por jogo (GBA-110, bloqueado).
- **Câmera além dos limites de mapa**: quando a câmera bate na borda do mapa, o jogo costuma parar de rolar; a margem mostra a borda, não mundo novo.

Conclusão honesta: o framework já entrega tudo o que é fisicamente dele — mapa affine até 1024, texto até 512 quando o jogo usa, sprites até a borda do viewport. O resto é conteúdo do título, e nenhum emulador/recompilador de referência resolveu isso genericamente (KNO-011 §Observação de fonte).

## Diagnóstico de margem (WS-32, medido)

`gba-run --margin-report` classifica cada coluna de margem estruturalmente, pela camada que a cobriu e por onde a amostra caiu (não por cor). Veredito no Rkanoid v3.00 (SHA `99ae82…3d94`), frame 400, `--view-width 320`:

| Região | backdrop | ring | authored |
|---|---|---|---|
| margem esquerda (40) | 0 | 40 | 0 |
| margem direita (40) | 0 | 24 | 16 |
| **total (80)** | **0** | **64** | **16** |

Interpretação: `BG0` é texto com mapa **256**, então `hw 256..279` enrola para o início da linha (ring) e `hw -40..-1` mostra a cauda do mapa (ring). As 16 colunas `hw 240..255` são conteúdo real do mapa — mas como o mapa é 256, são apenas as últimas 16 colunas da mesma tela. Não há mundo novo.

**Correção de uma medição anterior**: a estimativa "11716/12800 backdrop" estava errada porque comparava colunas de margem com a própria borda do recorte, um teste circular. A verificação correta compara `hw 256+k` com `hw k` dentro da superfície ampliada: repete (ring). E `hw 240..255` difere de `hw 0..15` em 297 de 2560 amostras, confirmando conteúdo real do mapa, não repetição.

Consequência para a SPEC-070: A-70-08 rejeita um teto acima de 256 quando o diagnóstico dá `ring`/`backdrop` majoritário. Para o Rkanoid, o teto correto é **240**; declarar 288 seria prometer margem que não existe.

## Questões abertas

- Qual título — se algum — justifica adapter de margem? Sem um alvo com SHA fixado, a fase de conteúdo não tem critério de aceite. Rkanoid demonstra o seam, não o recurso.
- A janela canônica deve continuar sendo o único artefato verificado (posição de gbarecomp e do vendor) ou, para jogos com adapter, a superfície ampliada também passa a ser artefato de verificação?
- `view_width` como chave aditiva em `settings.toml` (`version = 1`) ou incremento de versão com migração/backup (UI-02)? O parser atual rejeita chave desconhecida (`settings.rs:471`).
- Margem não-autorizada é backdrop (`palette[0]`, o que o hardware mostraria) ou preto (o que gbarecomp escolhe)?
- Ampliação vertical: o cabeçalho de gbarecomp reserva `kMaxExtraY = 352`, mas declara que linhas verticais exigem provedor autorizado. Fica fora desta spec.

## Promoção para specs e decisões

- [SPEC-069](../specs/SPEC-069-widescreen-presentation.md): requisitos `WS-69-*`, invariante de recorte canônico e matriz de aceite.
- [ADR-042](../adr/ADR-042-widescreen-presentation-surface.md): decisão entre ampliar a constante global e criar segunda superfície.
- [ROADMAP](../ROADMAP.md): GBA-105 (contrato e superfície) e GBA-110 (adapters por jogo) em `BACKLOG`.
- [SPEC-008](../specs/SPEC-008-game-profiles.md): a chave de teto por jogo pertence a `presentation_defaults`; a extensão ainda não foi escrita.
