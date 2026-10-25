# ADR-042 — Superfície de apresentação ampliada separada do framebuffer canônico

- Estado: Proposed
- Data: 2026-10-24
- Supersede: —
- Contrato: [SPEC-069](../specs/SPEC-069-widescreen-presentation.md)
- Levantamento: [KNO-011](../knowledge/KNO-011-widescreen-survey.md)

## Contexto

`crates/gba-core` produz um framebuffer BGR555 de 240×160 com largura fixa em `SCREEN_WIDTH` (`ppu.rs:4`). Essa constante não é só um número de apresentação: é o *stride* do framebuffer (`ppu.rs:29,74,189,224`), o tamanho dos buffers por linha (`bg_line`, `obj_line`, `objwin_line`, `ppu.rs:35-37`), o limite de `compose` (`ppu.rs:547`) e a base de `frame_hash` (`gba.rs:177-186`), que é o artefato de verificação exigido por VID-01/AV-01 e VAL-02.

Existe demanda por imagem mais larga que 3:2. O levantamento [KNO-011](../knowledge/KNO-011-widescreen-survey.md) mostra que: (a) nenhum emulador de referência implementa o recurso; (b) a única implementação aberta, `mstan/gbarecomp`, usa um segundo compositor opt-in e mantém o caminho fiel byte-idêntico "by construction"; (c) o vendor `JRickey/gba-recomp` mantém 240×160 em framebuffer, textura, buffers temporais e apresentação, mas sua camada de apresentação é agnóstica a `src_size` e não codifica 3:2 em código; (d) ampliar a largura do buffer por sonda local não alterou a imagem canônica em 1200 frames, mas revelou que a margem de um mapa de texto 256 é o **anel do tilemap**, não mundo novo (colunas 256–319 idênticas a `x-256`).

Duas forças opostas precisam ser acomodadas. Primeira: o contrato de fidelidade (SPEC-006 VID-01/VID-03, AV-01, SPEC-011 VAL-02) exige que a imagem canônica e seu hash não se movam, porque toda a verificação de AOT (`--verify-interpreter`, `frame_hash`, sweeps mGBA) depende disso. Segunda: uma imagem ampliada é uma preferência de host que não pode influenciar o guest, do mesmo modo que escala, filtro e correção de cor (SET-63-02).

Há também um limite técnico que nenhuma arquitetura contorna: BG de texto é amostrado de um mapa de 256 ou 512 px, bitmap modes são 240 px por construção (`ppu.rs:341`), e HUD/culling/câmera são decididos pelo código do jogo. Ampliar o renderer não produz mundo novo — produz repetição, backdrop e sprites estacionados reaparecendo.

## Decisão proposta

**1. O framebuffer canônico permanece 240×160, sem stride configurável.** `SCREEN_WIDTH`, `SCREEN_HEIGHT`, `Ppu::framebuffer`, `Gba::framebuffer()` e `Gba::frame_hash()` não mudam de contrato. Toda verificação continua definida neles.

**2. A imagem ampliada é uma segunda superfície, de responsabilidade do host.** Um `Ppu::view` opcional (margens esquerda/direita) habilita, por linha, um segundo destino de saída com largura maior. Com `view` zerado, o caminho é o atual e o resultado MUST ser byte-idêntico — a implementação não pode introduzir equivalência algébrica onde hoje há identidade.

**3. Margens são estado de apresentação, nunca de simulação.** Não entram em `frame_hash`, não alteram ciclos, não são serializadas, não chegam ao guest e não mudam `Gba::step`. Mudar `view` em runtime é permitido entre frames e MUST produzir o mesmo hash canônico.

**4. Ampliação é opt-in e por jogo.** O default é 240. A preferência persistida vale `min(preferência do usuário, teto do perfil)`. Perfil ausente ou jogo sem teto ⇒ 240, sem erro e sem fallback silencioso para uma largura "razoável". Adicionalmente, um *override de sessão* explícito (WS-69-18) pode forçar a largura acima do teto para a invocação corrente; ele é rotulado experimental, nunca é default, nunca é gravado e continua sem tocar hash ou ciclo. O override existe para tornar a superfície verificável antes de qualquer adapter; sem ele, o recurso ficaria invisível até GBA-110, que está bloqueado por falta de título-alvo.

**5. A margem é honesta por padrão.** Sem dados autorizados pelo jogo, a margem mostra o que o hardware mostraria se a linha fosse mais longa (wrap do mapa, backdrop) ou é recusada. Não há tentativa de inventar conteúdo: `compose` MUST NOT sintetizar pixels de mundo.

**6. Onde o desenho é indefinido, falha fechado.** Janela não uniforme na linha nativa ⇒ margem recusada. Bitmap ⇒ margem é backdrop. OBJ fora da faixa nativa ⇒ recorte nativo por padrão.

**7. `x >= 240 { x -= 512 }` é decodificação de OAM, não recorte, e não muda.** O recorte é a linha separada (`ppu.rs:446`), que ganha política explícita.

**8. A apresentação separa largura de textura e largura lógica.** `gba-desktop` passa a distinguir a largura da imagem enviada da largura lógica usada por overlay/touch (240×160). Overlay, menu, dica de toque e `window_to_logical` continuam em coordenadas guest.

## Alternativas

**A. Ampliar `SCREEN_WIDTH` globalmente.** Rejeitada. É a sonda WS-16: move o stride, dobra o custo de memória por linha, e põe o hash canônico sob suspeita de depender de uma preferência de host. O vendor e gbarecomp evitam esse caminho pela mesma razão; a tentativa de constante única no mGBA (#1960) é o registro público de que produz artefato. Além disso `SCREEN_WIDTH` aparece em `gba-run`/`gba-native-rt` (PPM) e nos testes de PPU, então o blast radius é o pipeline inteiro por um ganho que a própria sonda mostra ser repetição de tilemap.

**B. Manter 240×160 e escalar/esticar para preencher a janela.** Rejeitada como substituto, mas é o comportamento atual e permanece. Esticar distorce proporção e não adiciona informação; a spec trata apenas de *mais colunas*, não de aspecto forçado.

**C. Só por jogo, sem capacidade genérica no renderer.** Rejeitada como ponto de partida. Sem um caminho opt-in no PPU, cada jogo exigiria um fork do renderer. A capacidade genérica é pré-requisito barato; o conteúdo continua sendo por jogo.

**D. Adiar até um título concreto justificar.** Parcialmente adotada: o contrato é escrito agora porque toda a discussão futura depende dele, mas a implementação fica em backlog e o caminho de conteúdo só é aceito com um alvo de SHA fixado.

## Consequências

- Framebuffer, hashes, `--verify-interpreter`, sweeps e PPM de verificação ficam intocados; nenhuma evidência existente precisa ser re-gerada.
- O PPU ganha um segundo laço de saída sob demanda; custo medido é linear em colunas (WS-19: ~15% em 384). Um jogo em 240 não paga nada.
- `gba-desktop` precisa separar as duas larguras; até isso existir, a imagem ampliada não é apresentável, mesmo que o PPU a produza.
- A interface de usuário passa a ter uma preferência a mais em três superfícies (menu na janela, menu nativo, `gba-settings`) e um limite por perfil em `SPEC-008`.
- Existe risco real de o recurso ser percebido como quebrado quando a margem é wrap ou backdrop. A spec exige que o estado seja visível, e o default é 240.
- Ampliação vertical fica fora; nenhuma linha desta decisão autoriza mexer em `SCREEN_HEIGHT`.

## Critérios para revisitar

- Se um segundo caminho de renderer custar mais que ~20% no framebuffer canônico com `view` zerado, a decisão de manter um só pipeline precisa ser revista.
- Se o primeiro adapter por jogo exigir mudança no guest (tilemap 512, screenblocks), a SPEC-008 precisa de um capítulo de hooks de apresentação antes da implementação.
- Se surgir um alvo de referência com ampliação fiel e verificável (não apenas visual), comparar antes de expandir o contrato.
- Se `settings.toml` precisar de migração de versão para `view_width`, UI-02 passa a valer e esta decisão é reavaliada junto. Na entrega atual a chave é aditiva na versão 1, decisão registrada em PLAN-002.
- Se o override de sessão (WS-69-18) passar a ser usado por padrão em builds de terceiros, o rótulo experimental deixa de ser suficiente: nesse caso o override deve ser restringido a builds de desenvolvimento.

## Referências

[KNO-011](../knowledge/KNO-011-widescreen-survey.md) (WS-01–WS-20), [SPEC-006](../specs/SPEC-006-video-audio.md) (VID-01/VID-03/AV-01), [SPEC-011](../specs/SPEC-011-verification.md) (VAL-02), [SPEC-008](../specs/SPEC-008-game-profiles.md), [SPEC-063](../specs/SPEC-063-recompiled-frontend-settings.md) (SET-63-02), [KNO-006](../knowledge/KNO-006-vendor-gba-recomp.md), [ADR-041](ADR-041-interpreter-first-core.md).
