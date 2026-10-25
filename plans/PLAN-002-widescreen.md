# PLAN-002 — Superfície de apresentação ampliada (GBA-105)

Contrato: [SPEC-069](../specs/SPEC-069-widescreen-presentation.md). Decisão: [ADR-042](../adr/ADR-042-widescreen-presentation-surface.md). Evidência de base: [KNO-011](../knowledge/KNO-011-widescreen-survey.md). Status exclusivamente no [roadmap](../ROADMAP.md).

Escopo: `gba-core` (renderer), `gba-desktop` (apresentação, configuração, menu), `gba-settings` (aba de vídeo), `gba-run`/`gba-native-rt` (flag de override). **Fora**: Web/iOS/Android, pacote AOT ampliado, adapter de margem por jogo (GBA-110), ampliação vertical.

Decisões de escopo tomadas antes da sequência:

| Decisão | Escolha | Consequência |
|---|---|---|
| Ativação | Teto por jogo **mais** override de sessão explícito | O recurso é demonstrável antes de existir adapter; o override é experimental, não persistido e nunca default (WS-69-18) |
| Escopo do incremento | Core + desktop + configurações | Entrega jogável e verificável; Web fica com as amarras 3:2 atuais e é registrado como não verificado |
| `settings.toml` | Chave aditiva `[video] view_width` na versão 1 | Arquivos antigos continuam válidos; build antigo rejeita o novo com diagnóstico e preserva o original (UI-07) |

## Sequência

1. **Emenda de contrato.** WS-69-06 passa a reger a preferência persistida; WS-69-18 define o override de sessão; WS-69-19 define a fonte do teto (`Option<u16>` por SHA-256, hoje sempre `None`); T-29/T-30/T-31 entram na matriz.
2. **Core, identidade.** `View`, `MAX_VIEW_WIDTH = 384`, `Ppu::set_view`/`view_line`, `Gba::set_view`/`view_framebuffer`. Buffers de rascunho em `MAX_VIEW_WIDTH`, iteração em `view.width()`. Goldens de T-01 capturados **antes** da alteração.
3. **Core, texto.** `hw_x = x - left`, mosaic sobre `hw_x`, mapa enrolado com `rem_euclid`. T-02 (relação de anel, estrutural), T-03 (contraprova em mapa 512), T-04 (mosaic).
4. **Core, affine e bitmap.** Pré-avanço `(-left) * PA/PC`, wrap honrado, bitmap limitado ao nativo. T-05, T-06, T-07.
5. **Core, OBJ/janelas/composição.** Decodificação 9-bit intacta, recorte nativo ligado por padrão, janela resolvida em `hw_x` com falha fechada, margem recusada = backdrop. T-08 a T-12.
6. **Core, escrita canônica e diferencial.** Recorte `[left, left+240)` grava no framebuffer canônico; linha ampliada em `view_framebuffer`. T-13, T-14, T-20, T-21, T-22.
7. **Apresentação.** `Video` guarda largura de textura e `view_left`; presentação lógica usa a textura; `Painter`/`window_to_logical` deslocam por `view_left`. Smoke com driver dummy.
8. **Frontend.** `app.rs` consome `view_framebuffer`, aplica `set_view_width`, mantém overlay/menu/touch em 240×160.
9. **Configuração e UIs.** `view_width` aditivo, `view_ceiling(sha)` devolvendo `None`, precedência `override || min(preferência, teto)`, três UIs com valor efetivo e motivo, `--view-width` em desktop e runtime nativo. T-25 a T-31.
10. **Verificação e evidência.** Suíte, `fmt`, `diff --check`, `gba-run --hashes` nos dois modos, JSON em `knowledge/evidence/`, roadmap e lista explícita do que não foi verificado.

## Gates

| Gate | Critério objetivo |
|---|---|
| G-A identidade | T-01 verde sem alteração de goldens; suíte `gba-core` completa passa |
| G-B recorte | T-02 e T-03 verdes; T-20/T-21 com `frame_hash` idêntico frame a frame |
| G-C não-interferência | T-22 verde: ciclos e `instructions` idênticos com `view` on/off |
| G-D apresentação | Smoke dummy em 240, 320 e 384 sem erro; `presented` igual ao número de frames |
| G-E configuração | T-25 a T-31 verdes; `settings.toml` antigo carrega sem aviso novo |
| G-F entrega | Suíte alvo + `fmt --check` + `diff --check` limpos; evidência escrita; roadmap atualizado |

## Riscos

| Risco | Mitigação |
|---|---|
| Unificar o caminho 240 com o ampliado muda pixel | Goldens capturados de HEAD antes da primeira edição; T-01 é a barreira |
| Diferencial lento em debug | Rodar em `--release`, reduzir frames e registrar o custo real |
| Janela lógica desalinhada do toque | `view_left` explícito em `Painter`/`window_to_logical`; smoke cobre o caminho, verificação manual registrada como pendente |
| Margem lida como bug | Override rotulado experimental; default 240; margem de wrap documentada na evidência |
| Deriva de UI entre três superfícies | Mesma precedência em um único helper de settings; três pontos só apresentam o valor efetivo |
