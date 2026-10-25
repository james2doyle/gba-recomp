# SPEC-069 — Superfície de apresentação ampliada (widescreen) sobre framebuffer canônico

Estado do documento: Draft. Progresso de implementação: [ROADMAP](../ROADMAP.md).
Levantamento: [KNO-011](../knowledge/KNO-011-widescreen-survey.md). Decisão: [ADR-042](../adr/ADR-042-widescreen-presentation-surface.md).

Esta especificação define contrato, não implementação. Nada aqui deve ser lido como recurso disponível: o estado atual é 240×160 fixo em todo o pipeline.

## Problema, objetivos e escopo

Jogadores pedem imagem mais larga que a proporção 3:2 do GBA. O hardware não tem esse modo: a área visível é 240×160 e os mapas de BG de texto têm no máximo 512 px, com bitmap fixo em 240 px. Ampliar o renderer, portanto, produz tanto conteúdo legítimo (mapas affine/512, sprites posicionados fora da tela) quanto artefato (repetição do anel de 256 px, backdrop, sprites estacionados reaparecendo).

O objetivo desta spec é permitir uma **superfície de apresentação mais larga** sem tocar em nada do que hoje é verificado: framebuffer canônico, hash de frame, ciclos, `--verify-interpreter`, sweeps contra mGBA e fluxo AOT.

Objetivos verificáveis:

1. Uma imagem mais larga pode ser produzida e apresentada quando habilitada explicitamente.
2. A imagem canônica de 240×160 e seu hash permanecem idênticos byte a byte, com ou sem a ampliação.
3. O comportamento da margem é definido e previsível, e o recurso nunca "inventa" mundo.
4. Um jogo sem dados autorizados nunca muda de aparência por acidente.

## Fora de escopo

- Ampliação vertical (linhas além de 160). `SCREEN_HEIGHT` não muda.
- Conteúdo de margem autoral (tilemap de 64, screenblocks deslocados, HUD reposicionado, culling alargado). Isso depende de mudança no guest ou de adapter por jogo e pertence a [SPEC-008](SPEC-008-game-profiles.md) e a um incremento próprio.
- Esticar a imagem para preencher a janela, mudar proporção de aspecto declarada, ou "crop dinâmico".
- Aumento de resolução interna (supersampling de BG affine), que é outro recurso e outra spec.
- Rede: `SPEC-067` (party) e qualquer sincronização futura de cabo link. `view` não entra em estado de sessão.
- Web, iOS e Android: a preferência é definida, mas a apresentação ampliada nesses alvos depende dos adaptadores de `SPEC-068` e não é aceita por esta spec.

## Termos e invariantes

| Termo | Definição |
|---|---|
| **Framebuffer canônico** | `Ppu::framebuffer`, 240×160 BGR555, stride `SCREEN_WIDTH`. Único artefato de hash e verificação (VID-01, AV-01, VAL-02). |
| **Superfície ampliada** | Saída host-only com `240 + left + right` colunas × 160 linhas, produzida na mesma passagem de scanline. |
| **Margem** | Colunas da superfície ampliada cujo `x` de hardware está fora de `0..240`. |
| **x de hardware** | Coluna que o GBA teria desenhado. `hw_x = output_x - left`. Pode ser negativo. |
| **Recorte canônico** | Colunas `[left, left + 240)` de cada linha da superfície ampliada. |
| **Teto do jogo** | Largura máxima autorizada, atrelada ao SHA-256 da ROM. Ausente ⇒ 240. |
| **Recorte nativo de OBJ** | Política de não desenhar OBJ nas margens. Padrão ligado (ADR-042 §7). |

Invariantes:

- **INV-1 (identidade canônica):** com `view` zerado, a saída é bit-idêntica à implementação anterior. Não é "equivalente", é a mesma imagem.
- **INV-2 (recorte):** com `view` ativo, o recorte canônico de cada linha é bit-idêntico à linha canônica que a mesma passagem produziria com `view` zerado.
- **INV-3 (não-interferência):** `view` não altera ciclos guest, contagem de instruções, estado de CPU/bus, despacho nativo, misses ou guards.
- **INV-4 (não-persistência):** `view` nunca é serializado, salvo em save state, nem transmitido em rede.
- **INV-5 (decodificação ≠ recorte):** `x >= 240 → x -= 512` continua sendo decodificação do campo de 9 bits com sinal e não é afetado por esta spec. Só o teste de visibilidade muda.
- **INV-6 (sem síntese):** nenhum pixel de margem é gerado a partir de conhecimento sobre o jogo. Ou vem de amostragem das mesmas estruturas de hardware, ou é backdrop.

## Requisitos normativos

Cada requisito tem critério de aceite na matriz abaixo.

- **WS-69-01:** o framebuffer canônico, `frame_hash` e toda saída de verificação MUST permanecer em 240×160 BGR555 com stride 240. PPM de `gba-run` e `gba-native-rt`, `--hashes`, `--verify-interpreter` e comparação mGBA MUST continuar lendo exclusivamente esse framebuffer.
- **WS-69-02:** com `view` zerado, `render_line` MUST produzir exatamente a mesma linha de hoje. A implementação MUST NOT substituir o caminho atual por um caminho generalizado cuja igualdade dependa de álgebra; se o código for unificado, a igualdade MUST ser provada por teste de identidade byte a byte em todos os modos 0–5.
- **WS-69-03:** com `view` ativo, `Ppu` MUST produzir, por scanline, uma linha de `240 + left + right` pixels. O recorte canônico dessa linha MUST ser bit-idêntico à linha canônica correspondente (INV-2).
- **WS-69-04:** `view` MUST ser estado de apresentação: não MUST entrar em `frame_hash`, não MUST alterar `Gba::step`, ciclos, `instructions`, contadores nativos, nem ser serializado. Alterar `view` entre frames MUST ser permitido e MUST NOT alterar hashes canônicos de frames seguintes para os mesmos inputs.
- **WS-69-05:** geometria: `left`, `right` são colunas; largura total `240 + left + right` MUST ser limitada a 384 (16:9 ≈ 284, 2:1 = 320, 12:5 = 384). Pedido acima do teto MUST ser recusado com erro explícito no ponto de configuração, não truncado silenciosamente.
- **WS-69-06:** largura efetiva da preferência = `min(preferência do usuário, teto do jogo)`. Jogo sem teto conhecido ⇒ 240. Aplicar preferência acima do teto MUST resultar em 240 sem erro de execução, preservando a preferência do usuário para quando houver teto. Esta cláusula rege a preferência persistida, não o override de sessão de WS-69-18.
- **WS-69-07:** BG de texto nas margens MUST amostrar o mapa de hardware pela mesma regra de enrolamento do caminho canônico (`(x + hofs) mod largura_do_mapa`, largura 256 ou 512). Mosaic MUST ser aplicado sobre o `hw_x`, não sobre o `output_x`, para que a grade de blocos coincida com a nativa. Nenhuma extensão de mapa, repetição sintética ou espelhamento MUST ser gerada.
- **WS-69-08:** BG affine MUST extrapolar a partir do ponto de referência travado da linha, pré-avançando `(-left) * PA` / `(-left) * PC` e depois somando PA/PC por coluna. O bit de wrap (BGxCNT bit 13) MUST ser honrado; sem wrap, amostras fora da faixa MUST ser descartadas (backdrop).
- **WS-69-09:** em modos bitmap (3, 4, 5), a margem MUST ser backdrop. Nenhuma amostra fora da bitmap 240×160 (ou 160×128 no modo 5) MUST ser produzida.
- **WS-69-10:** OBJ: a decodificação de 9 bits com sinal MUST NOT mudar (INV-5). O recorte nativo MUST estar ligado por padrão: OBJ fora de `0..240` não MUST aparecer nas margens. Uma política explícita de "viewport ampliado" MAY ser oferecida, documentada como exceção, e MUST ser por jogo.
- **WS-69-11:** janelas: os testes de WIN0/WIN1 MUST usar `hw_x` limitado à faixa nativa `0..240`; a margem resolve para WINOUT. Se o controle de janela na linha nativa não for uniforme nas 240 colunas, a margem MUST ser backdrop (falha fechada). Nenhuma janela MUST ser estendida por interpolação ou por borda.
- **WS-69-12:** composição: colunas de margem MUST passar pela mesma pilha de prioridade, blending, brightness e mosaic do caminho nativo, com a máscara resolvida em WS-69-11. Não MUST existir um segundo compositor com regras próprias.
- **WS-69-13:** apresentação: `gba-desktop` MUST separar a largura da textura enviada (`240 + left + right`) da largura lógica de overlay, menu, dica de toque e `window_to_logical` (240×160) e MUST manter proporção correta com letterbox/pillarbox. A janela MUST NOT ser redimensionada como efeito colateral de mudar `view`.
- **WS-69-14:** configuração: `settings.toml` ganha `[video] view_width` inteiro entre 240 e 384, default 240. A chave MUST ser aditiva na versão 1 (arquivo antigo sem a chave continua válido; arquivo com a chave em build antigo é rejeitado com diagnóstico, comportamento atual de UI-07). Escrita atômica preservada.
- **WS-69-15:** UI: menu na janela, menu nativo macOS e `gba-settings` MUST expor a mesma opção, com o valor efetivo. Quando o jogo tem teto menor que a preferência, a UI MUST mostrar o valor efetivo e o motivo (`teto do jogo: N`).
- **WS-69-16:** adapter por jogo, quando existir, MUST ser dado versionado atrelado ao SHA-256, com evidência e sem script (SPEC-008 PROF-02/PROF-03). Nenhum adapter MUST ser ativado por título, game code ou nome de arquivo.
- **WS-69-17:** observabilidade: quando `view` for recusado ou limitado, o motivo MUST aparecer em log de diagnóstico e no relatório do pacote quando o relatório existir. Falha de configuração MUST NOT derrubar a execução do jogo.
- **WS-69-18:** override de sessão: a superfície ampliada MAY ser forçada para a sessão corrente, acima do teto do jogo e independentemente dele. O override MUST ser explícito por invocação (flag de linha de comando ou item separado de UI), MUST NOT ser default, MUST NOT ser persistido em `settings.toml`, perfil ou save, MUST NOT alterar `frame_hash` e MUST ser rotulado como experimental em toda superfície de UI que o exponha. Com o override inativo, aplica-se WS-69-06 sem exceção. O override MUST NOT ampliar além de `MAX_WIDTH` (WS-69-05).
- **WS-69-19:** fonte do teto: o teto por jogo MUST ser lido de dado versionado atrelado ao SHA-256 da ROM, exposto como `Option<u16>` (`None` = sem teto declarado ⇒ 240). Enquanto o parser de SPEC-008 não existir, o resolvedor MUST devolver `None` para toda ROM e MUST declarar essa condição na UI de diagnóstico; nenhum valor MAY ser inferido de título, game code, região, nome de arquivo ou tamanho da ROM.

## Interfaces, entradas e saídas

Interface pretendida (nomes podem mudar; a semântica é normativa):

```rust
/// Margens de apresentação por linha. Estado host-only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct View { pub left: u16, pub right: u16 }

impl View {
    pub const NONE: View = View { left: 0, right: 0 };
    /// Largura máxima de saída aceita por esta versão.
    pub const MAX_WIDTH: u16 = 384;
    pub fn width(self) -> u16;          // 240 + left + right
}

impl Ppu {
    /// Recusa (`Err`) quando `view.width() > View::MAX_WIDTH`; nunca aplica clamp.
    pub fn set_view(&mut self, view: View) -> Result<(), String>;
    pub fn view(&self) -> View;
    /// Linha do último `render_line` com `view().width()` pixels.
    /// Vazio quando `view() == View::NONE`.
    pub fn view_line(&self, y: usize) -> &[u16];
}

impl Gba {
    pub fn set_view(&mut self, view: View) -> Result<(), String>;
    /// Framebuffer ampliado, stride `view().width()`; vazio com `view == NONE`.
    pub fn view_framebuffer(&self) -> &[u16];
}
```

Entradas: preferência do usuário (`settings.toml`), teto do jogo (perfil, quando existir), tamanho de janela.
Saídas: superfície ampliada para o host; framebuffer canônico inalterado; diagnóstico quando há recusa/limite.

Não MUST existir caminho em que a superfície ampliada seja consumida por código guest, por ABI nativa (`gba-runtime-abi`, blocos AOT) ou por hash de verificação.

## Erros, limites e observabilidade

| Situação | Comportamento exigido |
|---|---|
| `view_width` fora de 240..=384 | Erro de configuração; defaults seguros; arquivo original preservado (UI-07) |
| `view_width` > teto do jogo | Efetivo = teto; sem erro; diagnóstico informativo |
| Jogo sem teto | Efetivo = 240; sem erro |
| `view` com largura > `MAX_WIDTH` na API | Erro explícito ao chamador (`Err`), nunca clamp silencioso |
| Janela não uniforme na linha | Margem backdrop; contador/telemetria de linhas recusadas quando houver diagnóstico |
| Bitmap modes | Margem backdrop; sem erro |
| Alvo sem suporte de apresentação (ex.: Web até adapter) | Preferência aceita, apresentação em 240; motivo registrado |

Nenhuma dessas condições MUST produzir fallback de execução, mudança de modo AOT ou reinício do guest.

## Critérios de aceite e matriz de testes

Unidade (`cargo test -p gba-core`), com fixture sintética no estilo de `ppu/tests.rs`:

| ID | Caso | Esperado |
|---|---|---|
| T-01 | `view = NONE` em modos 0–5, cena sintética | Linha idêntica byte a byte à implementação anterior (golden local) |
| T-02 | Texto com mapa 256, `left = 0`, `right = 128` (canvas 368) | Anel: coluna de saída `256+k` == coluna `k`, para todo `k` em `0..112` e toda linha (`hw_x = 256+k` enrola para `k`). Recorte canônico `[0, 240)` intacto. Colunas 240–255 não são asseridas: são `hw_x` 240–255, dentro do anel, e o conteúdo é dado do jogo |
| T-03 | Texto com mapa **512**, mesmas margens | Contraprova: coluna `256+k` MUST NOT ser igual a `k` (posições distintas do mapa 512). Falha se o mapa for tratado como 256 |
| T-04 | Mosaic H = 4, `left = 10` | Fronteiras de bloco coincidem com a grade nativa (snap em `hw_x`) |
| T-05 | Affine com `PA = PD = 0x0080`, referência deslocada, `left = 16` | Margem continua a amostragem esperada; sem wrap: fora da faixa ⇒ backdrop |
| T-06 | Affine com wrap ligado | Amostragem enrola em `size`, sem extensão sintética |
| T-07 | Modo 3/4/5 com `left = 32` | Margem inteira = backdrop |
| T-08 | OBJ em X bruto 248 e em X bruto 504 (`-8`), recorte nativo ON | Não visível na margem; aparece no nativo se estiver na faixa |
| T-09 | Mesmo caso, recorte nativo OFF (política ampliada) | Visível na margem direita/esquerda conforme `hw_x` |
| T-10 | WIN0 com `x2 = 240`, `left = 24` | Margem = WINOUT; sem extensão da janela |
| T-11 | WIN0 cobrindo parte da linha nativa | Margem = backdrop (falha fechada) |
| T-12 | Blending/alpha sobre BG nas margens | Mesmo resultado do cálculo nativo na coluna equivalente |
| T-13 | `View { left: 200, right: 200 }` via API | `Err` explícito, sem clamp |
| T-14 | `set_view` chamado entre frames | Hash canônico dos frames seguintes inalterado |

Diferencial e integração:

| ID | Caso | Esperado |
|---|---|---|
| T-20 | `gba-run` 1200 frames, mesmos inputs, `view` off vs on | `frame_hash` idêntico em todos os frames; recorte canônico idêntico |
| T-21 | Mesmo cenário, `view` alternando no meio | Nenhum frame divergente no recorte canônico |
| T-22 | Ciclos e instruções com `view` on/off | Idênticos |
| T-23 | Pacote `aot-compatible` com `--verify-interpreter` e `view` on | Sem divergência; misses/shells iguais ao cenário off |
| T-24 | PPM de `gba-run`/`gba-native-rt` com `view` on | Continua 240×160 |
| T-25 | `settings.toml` sem `view_width` | Carrega, default 240 |
| T-26 | `settings.toml` com `view_width = 385` e com `view_width = 200` | Erro de configuração nas duas; arquivo preservado; defaults seguros (UI-07) |
| T-27 | `view_width = 384` com jogo sem teto | Efetivo 240; diagnóstico presente |
| T-28 | Janela redimensionada e fullscreen com `view` ativo | Sem crash; proporção correta; overlay e toque alinhados ao recorte canônico |
| T-29 | Override de sessão ativo em jogo sem teto | Largura pedida aplicada; UI/CLI rotula experimental; nenhum valor gravado em `settings.toml` |
| T-30 | Preferência 320 com teto de jogo 288 | Efetivo 288; UI mostra `teto do jogo: 288`; preferência preservada como 320 |
| T-31 | Override e preferência alternados ao longo de N frames | `frame_hash` e ciclos idênticos ao cenário sem ampliação, em ambos os modos |

Critério de conclusão do contrato (não do recurso): a matriz T-01…T-28 deve existir como testes verdes, e a evidência deve registrar host, commit, ROM/hash e saída de `gba-run --hashes` para T-20/T-21. Conteúdo de margem autorizado (WS-69-16) fica para incremento separado, com evidência própria.

## Questões abertas

- Qual título justifica o primeiro adapter de margem? Sem alvo com SHA fixado não há critério de aceite para a fase de conteúdo (GBA-110).
- O override de sessão (WS-69-18) torna o recurso demonstrável antes de qualquer adapter, mas expõe margem de enrolamento/backdrop. Até existir teto por jogo, o rótulo experimental é a única proteção contra a leitura de que o recurso está quebrado. Decidir se o override deve exigir confirmação adicional na UI.
- Backdrop versus preto (`pillarbox`) como padrão de margem recusada. Esta spec fixa backdrop por consistência com o hardware; a preferência pode virar opção depois.
- `view_width` aditivo na versão 1 (decisão tomada em PLAN-002) versus `version = 2` com migração. A chave aditiva mantém arquivos antigos válidos; builds antigos rejeitam o arquivo novo com diagnóstico e preservam o original (UI-07). O caminho de migração continua disponível em UI-02 se a chave mudar de semântica.
- Se o incremento futuro de conteúdo exigir mudança no guest (tilemap 512, screenblocks), a SPEC-008 precisa de capítulo de hooks de apresentação antes de qualquer implementação.
- Netplay: se a fase 4 de `SPEC-067` sincronizar vídeo, é preciso decidir se clientes podem ter larguras diferentes. A expectativa é que `view` fique fora de estado de sessão.

## Referências e evidências

- [KNO-011](../knowledge/KNO-011-widescreen-survey.md): levantamento, WS-01–WS-20, incluindo as sondas locais (recorte canônico idêntico em 1200 frames; margem de mapa 256 é enrolamento; custo linear em colunas).
- [ADR-042](../adr/ADR-042-widescreen-presentation-surface.md): decisão e alternativas.
- [SPEC-006](SPEC-006-video-audio.md) VID-01/VID-03/AV-01; [SPEC-011](SPEC-011-verification.md) VAL-02; [SPEC-063](SPEC-063-recompiled-frontend-settings.md) SET-63-02/UI-07; [SPEC-008](SPEC-008-game-profiles.md) PROF-02/PROF-03; [SPEC-009](SPEC-009-settings-menu.md) UI-01/UI-02/UI-03.
- Fontes externas: `JRickey/gba-recomp@de4edf5` (contrato "hashes on raw BGR555, pipeline present-time only"); `mstan/gbarecomp` (segundo compositor opt-in, providers por jogo — referência de desenho, licença não permite reutilização); mGBA #1960/#2937 (limite de 256 px e artefatos da ampliação ingênua).
